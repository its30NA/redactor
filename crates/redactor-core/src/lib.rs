//! `redactor-core`: the detection engine behind redactor.
//!
//! Pure Rust, no I/O, no `unsafe`. Text goes in, redacted text and value-free findings come out.
#![forbid(unsafe_code)]

// Pipeline order: normalize -> detectors -> guards/threshold -> resolve -> redact.
mod classifier;
mod config;
mod context;
mod decode;
mod detector;
mod encoded;
mod engine;
mod finding;
mod guard;
mod normalize;
mod occurrences;
mod placeholder;
mod propagate;
mod redact;
mod resolve;
mod rules;
mod terms;
mod utf16;

pub use rules::{rule_infos, Family, RuleInfo};

pub use classifier::secret_probability;
pub use config::{CustomRule, EngineConfig, PlaceholderStyle, Sensitivity, DEFAULT_TEMPLATE};
pub use detector::Detector;
pub use engine::{Engine, EngineError, ScanRequest};
pub use finding::{
    Candidate, Finding, MatcherHit, OutputSpan, ScanOutcome, SecretMatcher, Source, Tier, ValueKey,
    Warning, WarningKind,
};
pub use normalize::is_hidden;
pub use utf16::Utf16Map;

/// The engine version (the crate version).
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
