//! Engine configuration.
//!
//! This module only defines the in-memory shape and its defaults. Loading `redactor.toml` lives in
//! the `scrub` crate (`crates/scrub/src/config.rs`).

use std::fmt;

/// How eager the engine is. Each level sets a confidence threshold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Sensitivity {
    /// Only confident findings (≥ 0.75).
    Relaxed,
    /// The default (≥ 0.50).
    #[default]
    Balanced,
    /// Everything plausible (≥ 0.30), plus the Paranoid-only detectors.
    Paranoid,
}

impl Sensitivity {
    /// Minimum confidence a candidate needs to survive at this level.
    pub fn threshold(self) -> f32 {
        match self {
            Sensitivity::Relaxed => 0.75,
            Sensitivity::Balanced => 0.50,
            Sensitivity::Paranoid => 0.30,
        }
    }
}

/// How placeholders are worded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum PlaceholderStyle {
    /// `[REDACTED: GitHub Token]`: the label says what was removed.
    #[default]
    Labeled,
    /// `[REDACTED: Secret]`: every placeholder uses the same label, so the output does not
    /// reveal which kinds of secrets were present.
    Generic,
}

/// A user-defined regex rule.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CustomRule {
    /// Detector id (shows up as `Finding::detector` and `Finding::kind`).
    pub id: String,
    /// Placeholder label.
    pub label: String,
    /// Regex (the `regex` crate syntax: no lookaround, no backreferences).
    pub pattern: String,
    /// Confidence in `[0, 1]`.
    pub confidence: f32,
    /// Capture group that holds the secret (0 = whole match).
    pub group: usize,
}

/// Everything that shapes an [`crate::Engine`]. Immutable once the engine is built.
#[derive(Clone)]
pub struct EngineConfig {
    pub sensitivity: Sensitivity,
    /// Detect personal data (email, phone, IBAN, card numbers, IPs, SSN).
    pub pii: bool,
    /// Remove invisible, bidi and tag characters from the output.
    pub strip_invisible: bool,
    /// Detector ids switched off.
    pub disabled: Vec<String>,
    /// Detector ids switched on that are off by default.
    pub enabled: Vec<String>,
    pub custom_rules: Vec<CustomRule>,
    /// Regexes tested against candidate values; a match drops the candidate.
    pub allowlist: Vec<String>,
    /// Always-redact words: plain names, company or host names. Matched
    /// case-insensitively as whole words; entries shorter than 3 characters are ignored.
    pub terms: Vec<String>,
    pub placeholder: PlaceholderStyle,
    /// Placeholder template with `{label}` and `{suffix}` (` #N` when a label has more than one value).
    pub template: String,
    /// Keys [`crate::ValueKey`]. Random per process by default; never printed.
    pub session_key: [u8; 32],
}

/// The default placeholder template.
pub const DEFAULT_TEMPLATE: &str = "[REDACTED: {label}{suffix}]";

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            sensitivity: Sensitivity::Balanced,
            pii: false,
            strip_invisible: true,
            disabled: Vec::new(),
            enabled: Vec::new(),
            custom_rules: Vec::new(),
            allowlist: Vec::new(),
            terms: Vec::new(),
            placeholder: PlaceholderStyle::Labeled,
            template: DEFAULT_TEMPLATE.to_string(),
            session_key: random_session_key(),
        }
    }
}

impl fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineConfig")
            .field("sensitivity", &self.sensitivity)
            .field("pii", &self.pii)
            .field("strip_invisible", &self.strip_invisible)
            .field("disabled", &self.disabled)
            .field("enabled", &self.enabled)
            .field("custom_rules", &self.custom_rules.len())
            .field("allowlist", &self.allowlist.len())
            .field("terms", &self.terms.len())
            .field("placeholder", &self.placeholder)
            .field("template", &self.template)
            .field("session_key", &"<hidden>")
            .finish()
    }
}

/// 32 random bytes without an extra dependency: the standard library seeds every
/// `RandomState` from the operating system's random source; we hash several of them
/// together with the clock and a counter through BLAKE3.
fn random_session_key() -> [u8; 32] {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut h = blake3::Hasher::new_derive_key("redactor-core v2 session key");
    for i in 0..4u64 {
        let mut s = std::collections::hash_map::RandomState::new().build_hasher();
        s.write_u64(i);
        h.update(&s.finish().to_le_bytes());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    h.update(&now.to_le_bytes());
    h.update(&COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    h.update(&std::process::id().to_le_bytes());
    *h.finalize().as_bytes()
}
