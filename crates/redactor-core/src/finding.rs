//! Result types. None of them carries a secret value.

use std::borrow::Cow;
use std::fmt;

/// How sure the engine is, bucketed for the UI: Certain ≥ 0.90, Likely ≥ 0.65, Possible below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Tier {
    Certain,
    Likely,
    Possible,
}

impl Tier {
    pub fn from_confidence(confidence: f32) -> Tier {
        if confidence >= 0.90 {
            Tier::Certain
        } else if confidence >= 0.65 {
            Tier::Likely
        } else {
            Tier::Possible
        }
    }
}

/// Which part of the pipeline produced a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Source {
    Pattern,
    Context,
    Classifier,
    Decoded,
    Watchlist,
    Propagated,
    Manual,
    Custom,
    Pii,
}

/// Opaque identity of a value: keyed BLAKE3(session key, normalized value), truncated to 16 bytes.
/// The UI sends it back in `ScanRequest::keep`; it cannot be turned back into the value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ValueKey(pub [u8; 16]);

impl ValueKey {
    /// Lowercase hex, 32 characters.
    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(32);
        for b in self.0 {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 15) as usize] as char);
        }
        s
    }

    /// Parses 32 hex characters (either case).
    pub fn from_hex(s: &str) -> Option<ValueKey> {
        let b = s.as_bytes();
        if b.len() != 32 {
            return None;
        }
        let nib = |c: u8| -> Option<u8> {
            match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            }
        };
        let mut out = [0u8; 16];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (nib(b[2 * i])? << 4) | nib(b[2 * i + 1])?;
        }
        Some(ValueKey(out))
    }
}

impl fmt::Debug for ValueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValueKey({})", self.to_hex())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ValueKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ValueKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <Cow<'de, str>>::deserialize(d)?;
        ValueKey::from_hex(&s)
            .ok_or_else(|| serde::de::Error::custom("valueKey must be 32 hex chars"))
    }
}

/// One redacted (or kept) span. Byte offsets into the INPUT text; never the value itself.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    /// Machine id of what was found (for catalog rules, the rule id).
    pub kind: String,
    /// Human label, also used in the placeholder.
    pub label: String,
    pub tier: Tier,
    pub confidence: f32,
    /// The detector (rule) that won the span.
    pub detector: String,
    /// Short, value-free explanation.
    pub reason: String,
    pub source: Source,
    pub value_key: ValueKey,
    /// The placeholder that replaced the span in the output (empty when kept).
    pub placeholder: String,
    /// The user chose to keep this value; it is not redacted.
    pub kept: bool,
}

/// Where a placeholder sits in the OUTPUT text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OutputSpan {
    pub start: usize,
    pub end: usize,
    /// Index into `ScanOutcome::findings`.
    pub finding: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum WarningKind {
    InvisibleChars,
    BidiControls,
    TagChars,
    DecodeLimit,
    VerifyCapHit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Warning {
    pub kind: WarningKind,
    pub count: usize,
}

/// The result of [`crate::Engine::scan`].
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ScanOutcome {
    /// Redacted text.
    pub output: String,
    /// Sorted by start; byte offsets into the input.
    pub findings: Vec<Finding>,
    /// Byte ranges of placeholders in `output`.
    pub output_spans: Vec<OutputSpan>,
    pub warnings: Vec<Warning>,
    /// False only if the verify loop hit its cap.
    pub verified: bool,
}

/// A range reported by a [`SecretMatcher`] (known secrets, e.g. the watchlist).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatcherHit {
    pub start: usize,
    pub end: usize,
    pub label: String,
}

/// Finds known secrets. Implemented by `redactor-watchlist`.
pub trait SecretMatcher: Send + Sync {
    /// Byte ranges in `text` (which is the engine's normalized shadow text).
    fn find(&self, text: &str) -> Vec<MatcherHit>;

    /// Every label `find` can give, so that placeholders carrying one are recognised when
    /// output is scanned again. A placeholder with a label not listed here is ordinary text.
    fn labels(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A detector's raw output, before guards, thresholds and resolution.
/// Offsets are byte offsets into the text the detector was given.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub start: usize,
    pub end: usize,
    pub kind: Cow<'static, str>,
    pub label: Cow<'static, str>,
    pub confidence: f32,
    pub detector: Cow<'static, str>,
    pub reason: Cow<'static, str>,
    pub source: Source,
}

impl Candidate {
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}
